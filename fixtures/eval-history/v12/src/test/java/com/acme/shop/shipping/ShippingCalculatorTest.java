package com.acme.shop.shipping;

import static org.junit.jupiter.api.Assertions.assertEquals;

import com.acme.shop.domain.Money;
import com.acme.shop.domain.Order;
import com.acme.shop.domain.Product;
import org.junit.jupiter.api.Test;

class ShippingCalculatorTest {
    @Test
    void freeFromExactlyTheThreshold() {
        Order order = new Order().add(new Product("chair", Money.ofCents(5000)), 1);
        assertEquals(Money.ZERO, new ShippingCalculator().shippingFor(order));
    }

    @Test
    void flatRateBelowTheThreshold() {
        Order order = new Order().add(new Product("pen", Money.ofCents(4999)), 1);
        assertEquals(Money.ofCents(495), new ShippingCalculator().shippingFor(order));
    }
}
