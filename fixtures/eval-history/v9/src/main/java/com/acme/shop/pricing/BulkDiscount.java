package com.acme.shop.pricing;

import com.acme.shop.domain.Money;
import com.acme.shop.domain.Order;

/** A percentage off orders of at least {@link #MIN_ITEMS} items. */
public final class BulkDiscount implements DiscountPolicy {
    static final int MIN_ITEMS = 10;

    private final int percent;

    public BulkDiscount(int percent) {
        if (percent < 0 || percent > 100) {
            throw new IllegalArgumentException("percent out of range: " + percent);
        }
        this.percent = percent;
    }

    @Override
    public Money discountFor(Order order) {
        boolean bulk = order.itemCount() >= MIN_ITEMS;
        if (bulk) {
            return order.subtotal().percent(percent);
        }
        return Money.ZERO;
    }
}
