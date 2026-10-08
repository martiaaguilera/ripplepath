package com.acme.shop.pricing;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

import com.acme.shop.domain.Money;
import com.acme.shop.domain.Order;
import com.acme.shop.domain.Product;
import org.junit.jupiter.api.Test;

class PercentageDiscountTest {
    @Test
    void takesPercentOfSubtotal() {
        Order order = new Order().add(new Product("lamp", Money.ofCents(1999)), 1);
        assertEquals(Money.ofCents(200), new PercentageDiscount(10).discountFor(order));
    }

    @Test
    void rejectsPercentAbove100() {
        assertThrows(IllegalArgumentException.class, () -> new PercentageDiscount(101));
    }
}
