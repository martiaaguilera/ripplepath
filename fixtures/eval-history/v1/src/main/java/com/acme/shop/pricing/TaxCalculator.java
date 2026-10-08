package com.acme.shop.pricing;

import com.acme.shop.domain.Money;
import java.io.IOException;
import java.io.InputStream;
import java.io.UncheckedIOException;
import java.util.Map;
import java.util.Properties;
import java.util.TreeMap;

/** VAT by destination country. Rates live in {@code tax-rates.properties} so finance can edit them. */
public final class TaxCalculator {
    private final Map<String, Integer> ratesByCountry;

    public TaxCalculator(Map<String, Integer> ratesByCountry) {
        this.ratesByCountry = new TreeMap<>(ratesByCountry);
    }

    public static TaxCalculator fromResource() {
        Properties properties = new Properties();
        try (InputStream in = TaxCalculator.class.getResourceAsStream("/tax-rates.properties")) {
            if (in == null) {
                throw new IllegalStateException("tax-rates.properties is missing from the classpath");
            }
            properties.load(in);
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
        Map<String, Integer> rates = new TreeMap<>();
        for (String country : properties.stringPropertyNames()) {
            rates.put(country, Integer.parseInt(properties.getProperty(country).trim()));
        }
        return new TaxCalculator(rates);
    }

    public int rateFor(String country) {
        Integer rate = ratesByCountry.get(country);
        if (rate == null) {
            throw new IllegalArgumentException("no tax rate for " + country);
        }
        return rate;
    }

    public Money taxOn(Money net, String country) {
        return net.percent(rateFor(country));
    }
}
