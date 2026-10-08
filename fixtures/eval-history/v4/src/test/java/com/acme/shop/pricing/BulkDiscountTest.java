package com.acme.shop.pricing;

import static org.junit.jupiter.api.Assertions.assertEquals;

import com.acme.shop.domain.Money;
import com.acme.shop.domain.Order;
import com.acme.shop.domain.Product;
import org.junit.jupiter.api.Test;

class BulkDiscountTest {
    private static final Product PEN = new Product("pen", Money.ofCents(100));

    @Test
    void appliesFromTenItems() {
        Order order = new Order().add(PEN, 10);
        assertEquals(Money.ofCents(150), new BulkDiscount(15).discountFor(order));
    }

    @Test
    void noDiscountBelowTenItems() {
        Order order = new Order().add(PEN, 9);
        assertEquals(Money.ZERO, new BulkDiscount(15).discountFor(order));
    }
}
