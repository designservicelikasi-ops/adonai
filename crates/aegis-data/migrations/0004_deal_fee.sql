-- Protocol 1.1: other broker fees (MT5 DEAL_FEE) reported with each deal.
ALTER TABLE deals ADD COLUMN fee REAL NOT NULL DEFAULT 0;
