package com.acme.shop.pricing;

import com.acme.shop.domain.Money;
import com.acme.shop.domain.Order;

/** A percentage off orders of at least {@link #MIN_ITEMS} items. */
public final class BulkDiscount implements DiscountPolicy {
    static final int MIN_ITEMS = 10;

    private final int percent;

    public BulkDiscount(int percent) {
        this.percent = percent;
    }

    @Override
    public Money discountFor(Order order) {
        if (order.itemCount() >= MIN_ITEMS) {
            return order.subtotal().percent(percent);
        }
        return Money.ZERO;
    }
}
