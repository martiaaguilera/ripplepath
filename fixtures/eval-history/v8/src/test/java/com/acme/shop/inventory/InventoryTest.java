package com.acme.shop.inventory;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

import org.junit.jupiter.api.Test;

class InventoryTest {
    @Test
    void reservesAvailableStock() {
        Inventory inventory = new Inventory();
        inventory.restock("book", 5);
        inventory.reserve("book", 2);
        assertEquals(3, inventory.available("book"));
    }

    @Test
    void refusesMoreThanAvailable() {
        Inventory inventory = new Inventory();
        inventory.restock("book", 1);
        assertThrows(OutOfStockException.class, () -> inventory.reserve("book", 2));
        assertEquals(1, inventory.available("book"));
    }

    @Test
    void allowsReservingExactlyAllStock() {
        Inventory inventory = new Inventory();
        inventory.restock("book", 3);
        inventory.reserve("book", 3);
        assertEquals(0, inventory.available("book"));
    }
}
