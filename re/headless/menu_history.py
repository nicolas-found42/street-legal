"""Read-only original menu snapshots at scheduler entry.

Layouts follow build606 FUN_00403e80 (class variables), FUN_00407da0
(instance variables), FUN_00403840 (name lookup), FUN_00416940 (thread list),
and FUN_0041fbc0/FUN_00407b10 (frame list), FUN_0040fb50 (sentinels),
and FUN_0041f650 (waiting-thread list). No callbacks or writes occur.
A snapshot at scheduler entry describes the preceding completed frame.
"""

MENU_CLASSES = {'java.game.MainMenu', 'java.game.MainMenuDialog', 'java.game.OptionsDialog'}


class MenuHistory:
    def __init__(self, observer):
        self.o = observer
        self.snapshots = []
        self.first_complete = None
        self.idle = 0
        self.state = None

    def variable(self, table, name):
        o = self.o
        vector = o.try_u32(table + 12) if table else None
        capacity = o.try_u32(vector) if vector else None
        count = o.try_u32(vector + 8) if vector else None
        data = o.try_u32(vector + 4) if vector else None
        if count is None or capacity is None or count > capacity or count > 4096 or (count and not data):
            return None
        found = None
        for index in range(count - 1, -1, -1):
            cell = o.try_u32(data + index * 4)
            pointer = o.try_u32(cell + 12) if cell else None
            if pointer and o.cstr(pointer) == name:
                if found is not None:
                    return None  # Ambiguous malformed variable table cannot certify state.
                found = {'cell': cell, 'value': o.try_u32(cell + 8),
                         'vector': vector, 'capacity': capacity, 'count': count}
        return found

    def chain(self, sentinel, maximum):
        o = self.o
        if not sentinel:
            return [], False
        tail = sentinel + 0x10
        item = o.try_u32(sentinel + 8)
        seen = set()
        out = []
        while item and item != tail:
            if item in seen or len(out) >= maximum:
                return out, False
            seen.add(item)
            following = o.try_u32(item + 4)
            if following is None:
                return out, False
            out.append(item)
            item = following
        return out, item == tail and o.try_u32(tail + 4) == 0

    def sample(self, jvm, iteration):
        o = self.o
        threads, valid = self.chain(o.try_u32(jvm + 0x18), 4096)
        waiting, good = self.chain(o.try_u32(jvm + 0x1C), 4096)
        valid &= good
        threads += waiting
        valid &= len(threads) == len(set(threads))
        pending = []
        frame_count = 0
        unknown_frames = []
        base_frames = []
        for thread in threads:
            frame_list = o.try_u32(thread + 0xC)
            frames, good = self.chain(frame_list, 4096)
            valid &= good
            for frame in frames:
                cls = o.try_u32(frame + 0x34)
                name = o.class_name(cls) if cls else None
                frame_count += 1
                if not name or name == '?':
                    # Thread::Thread creates one empty bottom frame with four zero
                    # constructor arguments (0041f340 -> 00407b10). Its name word
                    # is uninitialized, so it is not a script method frame.
                    base = (cls == 0 and o.frame_entries.get(frame) == 0
                            and o.try_u32(frame + 0x2C) == 0
                            and o.try_u32(frame + 0x30) == 0
                            and o.try_u32(frame + 0x3C) == thread
                            and o.try_u32(frame + 4) == frame_list + 0x10)
                    if base:
                        base_frames.append(frame)
                    else:
                        valid = False
                        unknown_frames.append(frame)
                if name in MENU_CLASSES:
                    pointer = o.try_u32(frame + 0x38)
                    pending.append({'thread': thread, 'frame': frame, 'class': name,
                                    'method': o.cstr(pointer) if pointer else None})
        candidates = []
        for cls, name in list(o.class_names.items()):
            if name == 'java.game.GameLogic':
                field = self.variable(o.try_u32(cls + 0x1D0), 'actualState')
                if field:
                    obj = field['value']
                    state_class = o.try_u32(obj + 0xC) if obj else None
                    candidates.append({**field, 'class': o.class_name(state_class) if state_class else None,
                                       'source': 'class_variables', 'owner': cls})
        complete = valid and len(candidates) == 1 and candidates[0]['class'] == 'java.game.MainMenu' and not pending
        state = candidates[0]['value'] if complete else None
        if complete:
            if self.first_complete is None or state != self.state:
                self.first_complete = iteration - 1
                self.idle = 0
            self.idle += 1
        else:
            self.first_complete = None
            self.idle = 0
        self.state = state
        record = {'iteration_completed': iteration - 1, 'icount': o.host.icount,
                  'native_calls': o.stats['native_calls'],
                  'filtered_records': o.natives_log.records if o.natives_log else None,
                  'all_records': o.all_log.records if o.all_log else None,
                  'thread_list_valid': valid, 'frame_count': frame_count,
                  'unknown_frames': unknown_frames, 'base_frames': base_frames, 'pending': pending, 'actual_state': candidates,
                  'complete': complete, 'consecutive_complete_samples': self.idle,
                  'idle_frames': max(0, self.idle - 1),
                  'first_complete_iteration': self.first_complete}
        self.snapshots.append(record)

    def sidecar(self):
        return {'alters_execution': False, 'sampling': 'scheduler_entry_prior_frame',
                'snapshots': self.snapshots}
