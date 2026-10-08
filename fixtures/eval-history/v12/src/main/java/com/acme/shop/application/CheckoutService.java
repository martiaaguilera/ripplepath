package com.acme.shop.application;

import com.acme.shop.domain.LineItem;
import com.acme.shop.domain.Money;
import com.acme.shop.domain.Order;
import com.acme.shop.inventory.Inventory;
import com.acme.shop.pricing.DiscountPolicy;
import com.acme.shop.pricing.TaxCalculator;
import com.acme.shop.shipping.ShippingCalculator;

public final class CheckoutService {
    private final Inventory inventory;
    private final DiscountPolicy discountPolicy;
    private final TaxCalculator taxCalculator;
    private final ShippingCalculator shippingCalculator;

    public CheckoutService(
            Inventory inventory,
            DiscountPolicy discountPolicy,
            TaxCalculator taxCalculator,
            ShippingCalculator shippingCalculator) {
        this.inventory = inventory;
        this.discountPolicy = discountPolicy;
        this.taxCalculator = taxCalculator;
        this.shippingCalculator = shippingCalculator;
    }

    public Receipt checkout(Order order, String country) {
        reserveStock(order);
        Money subtotal = order.subtotal();
        Money discount = discountPolicy.discountFor(order);
        Money net = subtotal.minus(discount);
        Money tax = taxCalculator.taxOn(net, country);
        Money shipping = shippingCalculator.shippingFor(order);
        return new Receipt(subtotal, discount, tax, shipping, net.plus(tax).plus(shipping));
    }

    private void reserveStock(Order order) {
        for (LineItem item : order.items()) {
            inventory.reserve(item.product().sku(), item.quantity());
        }
    }
}
