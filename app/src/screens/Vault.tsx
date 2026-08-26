import { useCallback, useEffect, useState } from 'react';

import { EntryDetailPane } from '../components/EntryDetail';
import { EntryList } from '../components/EntryList';
import { GroupRail } from '../components/GroupRail';
import { useAutoLock } from '../hooks/useAutoLock';
import { ipc } from '../ipc';
import type { EntrySummary, GroupNode, Uuid, VaultInfo } from '../ipc';

interface Props {
  info: VaultInfo;
  onLock: (reason: 'idle' | 'manual') => void;
}

/** Minutes of inactivity before the vault locks itself. */
const AUTO_LOCK_MINUTES = 5;

export function Vault({ info, onLock }: Props) {
  const [groups, setGroups] = useState<GroupNode | null>(null);
  const [group, setGroup] = useState<Uuid | undefined>(undefined);
  const [query, setQuery] = useState('');
  const [rows, setRows] = useState<EntrySummary[]>([]);
  const [selected, setSelected] = useState<Uuid | null>(null);
  const [loading, setLoading] = useState(true);

  useAutoLock(AUTO_LOCK_MINUTES, useCallback(() => onLock('idle'), [onLock]));

  useEffect(() => {
    void (async () => setGroups(await (await ipc()).groups()))();
  }, []);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    void (async () => {
      const api = await ipc();
      const next = await api.listEntries({
        ...(group ? { group } : {}),
        ...(query ? { query } : {}),
      });
      if (cancelled) return;
      setRows(next);
      setLoading(false);
      // Keep the selection only if it survived the filter, so the detail pane
      // never shows something the list no longer contains.
      setSelected((cur) => (cur && next.some((r) => r.uuid === cur) ? cur : null));
    })();
    return () => {
      cancelled = true;
    };
  }, [group, query]);

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === 'l' && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        onLock('manual');
      }
      if (e.key === 'Escape' && query) {
        setQuery('');
      }
    }
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onLock, query]);

  return (
    <div className="flex h-full" style={{ background: 'var(--window-bg)' }}>
      <GroupRail
        root={groups}
        selected={group}
        onSelect={setGroup}
        vaultName={info.name}
        onLock={() => onLock('manual')}
      />
      <EntryList
        rows={rows}
        loading={loading}
        query={query}
        onQuery={setQuery}
        selected={selected}
        onSelect={setSelected}
      />
      <EntryDetailPane uuid={selected} writable={info.writable} />
    </div>
  );
}
