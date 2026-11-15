-- Comment of the order that OPENED the position a deal closes (the AEGIS client
-- id for AEGIS orders). Lets reconciliation prove an order was executed even
-- when its position was opened and closed before any confirmation arrived.
ALTER TABLE deals ADD COLUMN origin TEXT NOT NULL DEFAULT '';
