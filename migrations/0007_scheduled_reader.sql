-- A reader selected while a book is being acquired belongs to that requester.
-- Freeze the address so changing a default or removing a reader does not
-- silently redirect an already scheduled delivery. Legacy Get & Send requests
-- keep their existing default-reader behavior until a destination is chosen.
ALTER TABLE acquisition_requests ADD COLUMN delivery_target_id INTEGER
    REFERENCES delivery_targets(id) ON DELETE SET NULL;
ALTER TABLE acquisition_requests ADD COLUMN delivery_address TEXT;
