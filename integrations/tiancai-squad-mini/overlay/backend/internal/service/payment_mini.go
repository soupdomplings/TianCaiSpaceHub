package service

import (
	"context"
	"errors"
	"math"
	"regexp"

	dbent "github.com/Wei-Shaw/sub2api/ent"
	"github.com/Wei-Shaw/sub2api/ent/paymentorder"
	"github.com/Wei-Shaw/sub2api/internal/payment"
)

const miniProvider = "wx_virtual"

var miniMerchant = regexp.MustCompile("^tc[0-9a-f]{28}$")

// All prices and credits come from the same policy used by the existing site cashier.
type MiniQuote struct {
	Payable float64 `json:"payable"`
	Credit  float64 `json:"credit"`
}

func (s *PaymentService) MiniQuoteOrCreate(ctx context.Context, uid int64, amount float64, merchant string, expected MiniQuote) (*MiniQuote, error) {
	req := CreateOrderRequest{UserID: uid, Amount: amount, PaymentType: miniProvider, OrderType: payment.OrderTypeBalance}
	if merchant != "" {
		if !miniMerchant.MatchString(merchant) {
			return nil, errors.New("invalid merchant order")
		}
		existing, err := s.entClient.PaymentOrder.Query().Where(paymentorder.OutTradeNoEQ(merchant)).Only(ctx)
		if err == nil {
			if existing.UserID != uid || existing.PaymentType != miniProvider || existing.PayAmount != expected.Payable || existing.Amount != expected.Credit {
				return nil, errors.New("merchant order belongs to a different purchase")
			}
			if existing.Status != OrderStatusPending {
				return nil, errors.New("order already processed or closed")
			}
			return &expected, nil
		}
		if !dbent.IsNotFound(err) {
			return nil, err
		}
	}
	cfg, err := s.configService.GetPaymentConfig(ctx)
	if err != nil {
		return nil, err
	}
	if !cfg.Enabled {
		return nil, errors.New("payment disabled")
	}
	if _, err = s.validateOrderInput(ctx, req, cfg); err != nil {
		return nil, err
	}
	policy := quoteRechargeBonus(cfg, amount, "CNY")
	_, payable, err := calculateCreateOrderPayAmountForOrderType(policy.PayBase, cfg.RechargeFeeRate, "CNY", req.OrderType, cfg.SubscriptionUSDToCNYRate)
	if err != nil {
		return nil, err
	}
	quote := &MiniQuote{Payable: payable, Credit: policy.Credited}
	if merchant == "" {
		return quote, nil
	}
	if *quote != expected {
		return nil, errors.New("quote changed")
	}
	if err = s.checkCancelRateLimit(ctx, uid, cfg); err != nil {
		return nil, err
	}
	user, err := s.userRepo.GetByID(ctx, uid)
	if err != nil {
		return nil, err
	}
	if user.Status != payment.EntityStatusActive {
		return nil, errors.New("account disabled")
	}
	req.miniMerchantID = merchant
	_, err = s.createOrderInTx(ctx, req, user, nil, cfg, policy.Credited, policy.PayBase, cfg.RechargeFeeRate,
		payable, policy.Bonus, &payment.InstanceSelection{ProviderKey: miniProvider})
	return quote, err
}

// Only the authenticated relay calls this after querying WeChat for this merchant order.
// Existing payment fulfillment owns the transactional balance credit and duplicate protection.
func (s *PaymentService) MiniSettle(ctx context.Context, merchant, trade string, paidCents int64) error {
	if !miniMerchant.MatchString(merchant) || trade == "" || len(trade) > 128 || paidCents <= 0 {
		return errors.New("invalid payment")
	}
	o, err := s.entClient.PaymentOrder.Query().Where(paymentorder.OutTradeNoEQ(merchant)).Only(ctx)
	if err != nil {
		return err
	}
	if o.PaymentType != miniProvider || int64(math.Round(o.PayAmount*100)) != paidCents ||
		(o.PaymentTradeNo != "" && o.PaymentTradeNo != trade) {
		return errors.New("payment mismatch")
	}
	if err = s.confirmPayment(ctx, o.ID, trade, float64(paidCents)/100, miniProvider, nil); err != nil {
		return err
	}
	current, err := s.entClient.PaymentOrder.Get(ctx, o.ID)
	if err != nil {
		return err
	}
	if current.Status != OrderStatusCompleted {
		return errors.New("credit pending")
	}
	return nil
}
