// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "./CommunityPool.sol";

/**
 * @title ParticipantPlan
 * @notice Individual payout plan contract controlled by a single creator wallet.
 *         - Deposits are held in wBTC
 *         - Thresholds and payout sizing are expressed in USDT but converted to wBTC via an oracle
 *         - Emergency exits and inactivity drains send wBTC to the shared CommunityPool
 */
contract ParticipantPlan {
    // --- Token + Oracle Interfaces ---

    interface IERC20 {
        function balanceOf(address account) external view returns (uint256);
        function transfer(address recipient, uint256 amount) external returns (bool);
        function allowance(address owner, address spender) external view returns (uint256);
        function approve(address spender, uint256 amount) external returns (bool);
        function transferFrom(address sender, address recipient, uint256 amount) external returns (bool);
        function decimals() external view returns (uint8);
    }

    interface AggregatorV3Interface {
        function decimals() external view returns (uint8);
        function latestRoundData()
            external
            view
            returns (
                uint80 roundId,
                int256 answer,
                uint256 startedAt,
                uint256 updatedAt,
                uint80 answeredInRound
            );
    }

    // --- Storage ---

    IERC20 public immutable wbtc;
    AggregatorV3Interface public immutable wbtcUsdOracle; // Expected to return wBTC price in USD with 8 decimals.
    CommunityPool public immutable communityPool;

    address public immutable creator; // Wallet that provided the initial deposit and is the exclusive payout recipient.
    uint256 public immutable thresholdUSDT; // 6-decimal USDT amount that triggers payout phase.
    uint256 public immutable monthlyPayoutUSDT; // 6-decimal USDT amount converted to wBTC at payout time.
    uint256 public immutable initialDeposit; // In wBTC wei.

    uint256 public totalDeposits; // All deposits (creator + others) in wBTC wei.
    uint256 public escrowBalance; // WBTC currently owned by the plan.
    uint256 public lastPayoutAt; // Timestamp of the last monthly payout trigger.
    uint256 public lastDepositAt; // Timestamp of the last deposit (creator or external).
    uint256 public inactiveDrainStart; // Timestamp when inactivity draining began (0 if not started).
    bool public inPayoutPhase; // True once threshold reached.
    bool public closed; // Plan is concluded (drained/emergency).

    uint256 private constant MONTH = 30 days;
    uint256 private constant PERCENT_DIVISOR = 10_000; // Basis points (1% = 100).

    // --- Events ---

    event DepositAdded(address indexed sender, uint256 amount);
    event PayoutTriggered(uint256 wbtcAmount, uint256 price, uint256 timestamp);
    event EmergencyExit(uint256 toCreator, uint256 toPool);
    event InactivityDrain(uint256 wbtcAmount, uint256 remaining);

    constructor(
        address _wbtc,
        address _wbtcUsdOracle,
        address _communityPool,
        uint256 _thresholdUSDT,
        uint256 _monthlyPayoutUSDT,
        uint256 _initialDeposit
    ) {
        require(_wbtc != address(0) && _wbtcUsdOracle != address(0) && _communityPool != address(0), "zero address");
        require(_thresholdUSDT > 0 && _monthlyPayoutUSDT > 0, "invalid config");
        require(_initialDeposit > 0, "initial deposit required");

        wbtc = IERC20(_wbtc);
        wbtcUsdOracle = AggregatorV3Interface(_wbtcUsdOracle);
        communityPool = CommunityPool(_communityPool);

        creator = msg.sender;
        thresholdUSDT = _thresholdUSDT;
        monthlyPayoutUSDT = _monthlyPayoutUSDT;
        initialDeposit = _initialDeposit;

        // Pull wBTC from creator
        _pullToken(wbtc, msg.sender, address(this), _initialDeposit);
        totalDeposits = _initialDeposit;
        escrowBalance = _initialDeposit;
        lastPayoutAt = block.timestamp;
        lastDepositAt = block.timestamp;

        if (_usdtValueOfWbtc(totalDeposits) >= thresholdUSDT) {
            inPayoutPhase = true;
        }

        // Register with community pool (eligible only if not yet in payout phase)
        communityPool.registerPlan(creator, initialDeposit, !inPayoutPhase);

        emit DepositAdded(msg.sender, _initialDeposit);
    }

    // --- Plan lifecycle ---

    /**
     * @notice Allows anyone to add wBTC to this plan.
     */
    function contribute(uint256 amount) external {
        require(!closed, "plan closed");
        require(amount > 0, "amount required");

        _pullToken(wbtc, msg.sender, address(this), amount);
        totalDeposits += amount;
        escrowBalance += amount;
        lastDepositAt = block.timestamp;

        if (!inPayoutPhase && _usdtValueOfWbtc(totalDeposits) >= thresholdUSDT) {
            inPayoutPhase = true;
            communityPool.setEligibility(false);
        }

        emit DepositAdded(msg.sender, amount);
    }

    // --- Payout logic ---

    /**
     * @notice Creator triggers a monthly payout once in the payout phase.
     */
    function triggerMonthlyPayout() external {
        require(msg.sender == creator, "only creator");
        require(!closed, "plan closed");
        require(inPayoutPhase, "threshold not met");
        require(block.timestamp >= lastPayoutAt + MONTH, "too soon");

        uint256 wbtcAmount = _convertUSDTToWbtc(monthlyPayoutUSDT);
        if (wbtcAmount > escrowBalance) {
            wbtcAmount = escrowBalance;
        }
        require(wbtcAmount > 0, "nothing to withdraw");

        lastPayoutAt = block.timestamp;
        inactiveDrainStart = 0; // reset inactivity schedule

        _pushToken(wbtc, creator, wbtcAmount);
        escrowBalance -= wbtcAmount;
        emit PayoutTriggered(wbtcAmount, _latestWbtcUsd(), block.timestamp);

        if (escrowBalance == 0) {
            closed = true;
            communityPool.removePlan();
        }
    }

    /**
     * @notice Emergency exit: 90% to creator, 10% to community pool.
     */
    function emergencyExit() external {
        require(msg.sender == creator, "only creator");
        require(!closed, "plan closed");

        uint256 balance = escrowBalance;
        require(balance > 0, "no funds");

        uint256 toCreator = (balance * 9000) / PERCENT_DIVISOR; // 90%
        uint256 toPool = balance - toCreator; // 10%

        closed = true;
        escrowBalance = 0;

        _pushToken(wbtc, creator, toCreator);
        _pushToken(wbtc, address(communityPool), toPool);
        communityPool.notifyInbound(toPool);
        communityPool.removePlan();

        emit EmergencyExit(toCreator, toPool);
    }

    /**
     * @notice Anyone can start or continue inactivity draining after 90 days without participant activity.
     *         10% of the current balance is moved to the community pool per month until the plan is empty or the creator resumes payouts.
     */
    function drainInactive() external {
        require(!closed, "plan closed");
        require(block.timestamp >= lastPayoutAt + 90 days, "not inactive");

        if (inactiveDrainStart == 0) {
            inactiveDrainStart = block.timestamp;
        } else {
            require(block.timestamp >= inactiveDrainStart + MONTH, "drain too soon");
            inactiveDrainStart = block.timestamp;
        }

        uint256 balance = escrowBalance;
        require(balance > 0, "empty");

        uint256 toPool = (balance * 1000) / PERCENT_DIVISOR; // 10%
        if (toPool == 0) {
            toPool = balance;
        }

        escrowBalance -= toPool;
        _pushToken(wbtc, address(communityPool), toPool);
        communityPool.notifyInbound(toPool);
        emit InactivityDrain(toPool, escrowBalance);

        if (escrowBalance == 0) {
            closed = true;
            communityPool.removePlan();
        }
    }

    // --- View helpers ---

    function planBalance() external view returns (uint256) {
        return escrowBalance;
    }

    // --- Internal helpers ---

    function _convertUSDTToWbtc(uint256 amountUSDT) internal view returns (uint256) {
        uint256 price = _latestWbtcUsd(); // 8 decimals typically
        uint8 priceDecimals = wbtcUsdOracle.decimals();
        uint8 wbtcDecimals = wbtc.decimals();
        require(price > 0, "oracle error");
        require(priceDecimals >= 6, "oracle decimals");

        uint256 numerator = amountUSDT * (10 ** wbtcDecimals) * (10 ** priceDecimals);
        return numerator / price / (10 ** 6);
    }

    function _usdtValueOfWbtc(uint256 amountWbtc) internal view returns (uint256) {
        uint256 price = _latestWbtcUsd();
        uint8 priceDecimals = wbtcUsdOracle.decimals();
        uint8 wbtcDecimals = wbtc.decimals();
        require(priceDecimals >= 6, "oracle decimals");
        return (amountWbtc * price) / (10 ** wbtcDecimals) / (10 ** (priceDecimals - 6));
    }

    function _latestWbtcUsd() internal view returns (uint256) {
        (, int256 answer,, uint256 updatedAt,) = wbtcUsdOracle.latestRoundData();
        require(answer > 0 && updatedAt + 1 days > block.timestamp, "stale oracle");
        return uint256(answer);
    }

    function _pullToken(IERC20 token, address from, address to, uint256 amount) internal {
        require(token.transferFrom(from, to, amount), "transferFrom failed");
    }

    function _pushToken(IERC20 token, address to, uint256 amount) internal {
        require(token.transfer(to, amount), "transfer failed");
    }
}
