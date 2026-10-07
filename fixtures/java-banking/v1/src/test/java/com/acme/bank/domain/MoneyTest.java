package com.acme.bank.domain;

import static org.junit.jupiter.api.Assertions.assertTrue;

import org.junit.jupiter.api.Test;

class MoneyTest {
    @Test
    void subtractingMoreThanBalanceIsNegative() {
        Money result = Money.of("1.00", "EUR").minus(Money.of("2.00", "EUR"));
        assertTrue(result.isNegative());
    }
}
