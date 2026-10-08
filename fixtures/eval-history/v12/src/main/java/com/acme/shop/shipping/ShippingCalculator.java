package com.acme.shop.shipping;

import com.acme.shop.domain.Money;
import com.acme.shop.domain.Order;

public final class ShippingCalculator {
    static final Money FREE_SHIPPING_FROM = Money.ofCents(5000);
    static final Money FLAT_RATE = Money.ofCents(495);

    /** Orders of 50.00 EUR or more ship for free. */
    public Money shippingFor(Order order) {
        Money subtotal = order.subtotal();
        if (subtotal.compareTo(FREE_SHIPPING_FROM) >= 0) {
            return Money.ZERO;
        }
        return FLAT_RATE;
    }
}
