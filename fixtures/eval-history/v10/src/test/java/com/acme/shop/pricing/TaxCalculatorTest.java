package com.acme.shop.pricing;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

import com.acme.shop.domain.Money;
import java.util.Map;
import org.junit.jupiter.api.Test;

class TaxCalculatorTest {
    @Test
    void loadsRatesFromResource() {
        TaxCalculator calculator = TaxCalculator.fromResource();
        assertEquals(21, calculator.rateFor("ES"));
        assertEquals(19, calculator.rateFor("DE"));
    }

    @Test
    void taxIsRateOfNetAmount() {
        TaxCalculator calculator = new TaxCalculator(Map.of("ES", 21));
        assertEquals(Money.ofCents(210), calculator.taxOn(Money.ofCents(1000), "ES"));
    }

    @Test
    void unknownCountryIsRefused() {
        TaxCalculator calculator = new TaxCalculator(Map.of("ES", 21));
        assertThrows(IllegalArgumentException.class, () -> calculator.rateFor("XX"));
    }
}
