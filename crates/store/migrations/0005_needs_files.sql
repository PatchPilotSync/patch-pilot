-- Schema v5: the files each file needs beside it to work (decision #62),
-- such as a script's libraries or a theme's images, as a JSON array of
-- `/`-separated paths below the same root. NULL when it needs none.
-- Earlier builds didn't keep them, so Bring over couldn't keep packages whole.

ALTER TABLE items ADD COLUMN needs_files TEXT;
