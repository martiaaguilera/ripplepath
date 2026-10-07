CREATE TABLE accounts (
    id VARCHAR(64) PRIMARY KEY,
    balance NUMERIC(19, 4) NOT NULL,
    currency CHAR(3) NOT NULL
);
