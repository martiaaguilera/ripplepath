package com.acme.shop.domain;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

import org.junit.jupiter.api.Test;

class OrderTest {
    @Test
    void subtotalSumsLineItems() {
        Order order = new Order()
                .add(new Product("book", Money.ofCents(1250)), 2)
                .add(new Product("pen", Money.ofCents(199)), 3);
        assertEquals(Money.ofCents(3097), order.subtotal());
    }

    @Test
    void rejectsNonPositiveQuantity() {
        Order order = new Order();
        Product pen = new Product("pen", Money.ofCents(199));
        assertThrows(IllegalArgumentException.class, () -> order.add(pen, 0));
    }
}
