package com.acme.shop.domain;

import static org.junit.jupiter.api.Assertions.assertEquals;

import org.junit.jupiter.api.Test;

class MoneyTest {
    @Test
    void addsAndSubtracts() {
        assertEquals(Money.ofCents(350), Money.ofCents(200).plus(Money.ofCents(150)));
        assertEquals(Money.ofCents(50), Money.ofCents(200).minus(Money.ofCents(150)));
    }

    @Test
    void minusNeverGoesBelowZero() {
        assertEquals(Money.ZERO, Money.ofCents(100).minus(Money.ofCents(250)));
    }

    @Test
    void percentRoundsHalfUp() {
        assertEquals(Money.ofCents(53), Money.ofCents(250).percent(21));
        assertEquals(Money.ofCents(200), Money.ofCents(1999).percent(10));
    }

    @Test
    void formatsAsEuros() {
        assertEquals("12.34 EUR", Money.ofCents(1234).toString());
    }
}
