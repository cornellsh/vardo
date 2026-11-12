/**
 * Locks, saving unsaved edits first (`inv:lock-drops-plaintext`).
 *
 * Saving is the safe direction: the write is verified and reversible through
 * history, while discarding work is neither. A failed save does not lock,
 * because locking would then be what loses the data. Returns whether it locked.
 */
export async function saveThenLock(
	dirty: boolean,
	save: () => Promise<void>,
	lock: () => void,
): Promise<boolean> {
	try {
		if (dirty) await save();
	} catch {
		return false;
	}
	lock();
	return true;
}
