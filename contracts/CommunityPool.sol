// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/**
 * @title CommunityPool
 * @notice Holds pooled wBTC from participant plans (inactive drains or emergency exits) and distributes it
 *         monthly to eligible participants plus a provider fee and fee buffer.
 *
 * Distribution split:
 * - 1% provider
 * - 98% to plans that have NOT reached their payout phase (weighted by initial deposit)
 * - 1% retained in the pool for transaction fees
 *
 * Eligibility and weights are managed by the individual plan contracts through callbacks.
 */
contract CommunityPool {
    // --- Interfaces ---
    interface IERC20 {
        function balanceOf(address account) external view returns (uint256);
        function transfer(address recipient, uint256 amount) external returns (bool);
    }

    // --- Storage ---
    IERC20 public immutable wbtc;
    address public immutable providerWallet;

    uint256 private constant PERCENT_DIVISOR = 10_000; // 1% = 100

    struct PlanInfo {
        address creator;
        uint256 weight; // Initial deposit (or other agreed weight)
        bool eligible; // true if the plan has NOT entered payout phase
        bool exists;
    }

    mapping(address => PlanInfo) public plans; // plan contract address => info
    uint256 public communityBalance; // wBTC tracked for distribution (should be <= on-chain balance)

    // --- Events ---
    event PlanRegistered(address indexed plan, address indexed creator, uint256 weight, bool eligible);
    event PlanEligibilityUpdated(address indexed plan, bool eligible);
    event PlanRemoved(address indexed plan);
    event PoolIncreased(address indexed plan, uint256 amount, uint256 newBalance);
    event PoolDistributed(uint256 total, uint256 toProvider, uint256 toParticipants, uint256 retainedForFees);

    constructor(address _wbtc, address _providerWallet) {
        require(_wbtc != address(0) && _providerWallet != address(0), "zero address");
        wbtc = IERC20(_wbtc);
        providerWallet = _providerWallet;
    }

    // --- Plan management (only callable by plan contracts) ---

    function registerPlan(address creator, uint256 weight, bool eligible) external {
        require(!plans[msg.sender].exists, "already registered");
        require(creator != address(0), "creator required");
        plans[msg.sender] = PlanInfo({creator: creator, weight: weight, eligible: eligible, exists: true});
        emit PlanRegistered(msg.sender, creator, weight, eligible);
    }

    function setEligibility(bool eligible) external {
        PlanInfo storage info = _requirePlan(msg.sender);
        info.eligible = eligible;
        emit PlanEligibilityUpdated(msg.sender, eligible);
    }

    function removePlan() external {
        PlanInfo storage info = _requirePlan(msg.sender);
        delete plans[msg.sender];
        emit PlanRemoved(msg.sender);
    }

    /**
     * @notice Plans call this after transferring wBTC to the pool.
     */
    function notifyInbound(uint256 amount) external {
        _requirePlan(msg.sender);
        require(amount > 0, "amount required");
        communityBalance += amount;
        require(wbtc.balanceOf(address(this)) >= communityBalance, "insufficient wbtc");
        emit PoolIncreased(msg.sender, amount, communityBalance);
    }

    // --- Distribution ---

    function distribute(address[] calldata candidatePlans) external {
        uint256 pool = communityBalance;
        require(pool > 0, "nothing to distribute");
        require(wbtc.balanceOf(address(this)) >= pool, "insufficient wbtc");

        uint256 totalWeight;
        uint256[] memory weights = new uint256[](candidatePlans.length);
        address[] memory creators = new address[](candidatePlans.length);

        for (uint256 i = 0; i < candidatePlans.length; i++) {
            PlanInfo memory info = plans[candidatePlans[i]];
            if (!info.exists || !info.eligible || info.weight == 0) continue;
            weights[i] = info.weight;
            creators[i] = info.creator;
            totalWeight += info.weight;
        }
        require(totalWeight > 0, "no eligible plans");

        uint256 toProvider = (pool * 100) / PERCENT_DIVISOR; // 1%
        uint256 toParticipants = (pool * 9800) / PERCENT_DIVISOR; // 98%
        uint256 retainedForFees = pool - toProvider - toParticipants; // 1%

        communityBalance = retainedForFees;

        require(wbtc.transfer(providerWallet, toProvider), "provider transfer failed");

        for (uint256 i = 0; i < candidatePlans.length; i++) {
            if (weights[i] == 0) continue;
            uint256 share = (toParticipants * weights[i]) / totalWeight;
            if (share > 0) {
                require(wbtc.transfer(creators[i], share), "participant transfer failed");
            }
        }

        emit PoolDistributed(pool, toProvider, toParticipants, retainedForFees);
    }

    // --- Internal ---

    function _requirePlan(address plan) internal view returns (PlanInfo storage info) {
        info = plans[plan];
        require(info.exists, "unknown plan");
    }
}
