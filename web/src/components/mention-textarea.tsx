import { useMemo, useRef, useState, type KeyboardEvent, type TextareaHTMLAttributes } from 'react';

export type MentionItem = {
  id: string;
  label: string;
  title: string;
  kind: string;
};

type Props = Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, 'onChange' | 'value'> & {
  value: string;
  items: MentionItem[];
  onChange: (value: string) => void;
  onMention?: (item: MentionItem) => void;
};

export function MentionTextarea({ value, items, onChange, onMention, onKeyDown, ...props }: Props) {
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const [query, setQuery] = useState<string | null>(null);
  const [start, setStart] = useState(0);
  const [active, setActive] = useState(0);
  const matches = useMemo(() => {
    if (query === null) return [];
    const needle = query.toLowerCase();
    return items.filter((item) => `${item.label} ${item.title} ${item.kind}`.toLowerCase().includes(needle)).slice(0, 8);
  }, [items, query]);

  const insert = (item: MentionItem) => {
    const end = textareaRef.current?.selectionStart ?? value.length;
    const next = `${value.slice(0, start)}@${item.label} ${value.slice(end)}`;
    onChange(next);
    onMention?.(item);
    setQuery(null);
  };

  return (
    <div className="mention-textarea">
      <textarea
        {...props}
        ref={textareaRef}
        value={value}
        onChange={(event) => {
          const next = event.currentTarget.value;
          const cursor = event.currentTarget.selectionStart ?? next.length;
          onChange(next);
          const prefix = next.slice(0, cursor);
          const match = /(^|\s)@([^\s@]*)$/.exec(prefix);
          if (!match || items.length === 0) {
            setQuery(null);
            return;
          }
          setStart(cursor - match[2].length - 1);
          setQuery(match[2]);
          setActive(0);
        }}
        onKeyDown={(event: KeyboardEvent<HTMLTextAreaElement>) => {
          if (query !== null && matches.length > 0) {
            if (event.key === 'ArrowDown') {
              event.preventDefault();
              setActive((index) => (index + 1) % matches.length);
              return;
            }
            if (event.key === 'ArrowUp') {
              event.preventDefault();
              setActive((index) => (index - 1 + matches.length) % matches.length);
              return;
            }
            if (event.key === 'Enter' && !event.nativeEvent.isComposing) {
              event.preventDefault();
              const item = matches[active];
              if (item) insert(item);
              return;
            }
            if (event.key === 'Escape') {
              event.preventDefault();
              setQuery(null);
              return;
            }
          }
          onKeyDown?.(event);
        }}
      />
      {query !== null && matches.length > 0 ? (
        <div className="mention-menu" role="listbox">
          {matches.map((item, index) => (
            <button
              key={item.id}
              className={index === active ? 'is-active' : undefined}
              onMouseDown={(event) => {
                event.preventDefault();
                insert(item);
              }}
              type="button"
            >
              <strong>{item.label}</strong>
              <small>{item.kind}</small>
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}
