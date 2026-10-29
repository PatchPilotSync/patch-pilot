-- Schema v9: each machine's drives and how full they are (decision #101).
--
-- JSON, as `MachineInfo.disks` serializes. Kept beside the machine rather
-- than in a snapshot: free space moves all the time and isn't what the
-- machine has, so it never makes a new snapshot.

ALTER TABLE machines ADD COLUMN disks TEXT;
