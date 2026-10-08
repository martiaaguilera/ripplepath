package com.acme.shop.pricing;

import com.acme.shop.domain.Money;
import com.acme.shop.domain.Order;

public final class PercentageDiscount implements DiscountPolicy {
    private final int percent;

    public PercentageDiscount(int percent) {
        if (percent < 0 || percent > 100) {
            throw new IllegalArgumentException("percent out of range: " + percent);
        }
        this.percent = percent;
    }

    @Override
    public Money discountFor(Order order) {
        return order.subtotal().percent(percent);
    }
}
