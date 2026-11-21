-- v0.3: micro-profit brain. `probability_raw` is the model's probability BEFORE the
-- brain's calibration haircut (the haircut is learned from it; learning it from the
-- already-reduced value would compound). `brain` = 1 when the brain chose the target.
ALTER TABLE trade_journal ADD COLUMN probability_raw REAL;
ALTER TABLE trade_journal ADD COLUMN brain INTEGER NOT NULL DEFAULT 0;
