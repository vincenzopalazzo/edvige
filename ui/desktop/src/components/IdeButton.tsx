import { useEffect, useState } from 'react';
import { ChevronDown } from 'lucide-react';
import { Button } from './ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from './ui/dropdown-menu';
import { type DetectedIde, type IdeId } from '../types/ide';
import { defineMessages, useIntl } from '../i18n';
import { toastError } from '../toasts';

const i18n = defineMessages({
  openIn: {
    id: 'ideButton.openIn',
    defaultMessage: 'Open in {name}',
  },
  chooseDefault: {
    id: 'ideButton.chooseDefault',
    defaultMessage: 'Choose default IDE',
  },
  defaultLabel: {
    id: 'ideButton.defaultLabel',
    defaultMessage: 'default',
  },
  openFailed: {
    id: 'ideButton.openFailed',
    defaultMessage: 'Failed to open project in IDE',
  },
  saveFailed: {
    id: 'ideButton.saveFailed',
    defaultMessage: 'Failed to save default IDE',
  },
  fallbackName: {
    id: 'ideButton.fallbackName',
    defaultMessage: 'IDE',
  },
});

interface IdeButtonProps {
  workingDir: string;
}

export function IdeButton({ workingDir }: IdeButtonProps) {
  const intl = useIntl();
  const [ides, setIdes] = useState<DetectedIde[]>([]);
  const [selectedId, setSelectedId] = useState<IdeId | null>(null);

  useEffect(() => {
    let cancelled = false;

    async function load() {
      const [detected, savedId] = await Promise.all([
        window.electron.listDetectedIdes().catch(() => []),
        window.electron.getSetting('GOOSE_DEFAULT_IDE').catch(() => ''),
      ]);
      if (cancelled) return;
      setIdes(detected);
      setSelectedId(
        detected.some((ide) => ide.id === savedId) ? (savedId as IdeId) : (detected[0]?.id ?? null)
      );
    }

    void load();
    return () => {
      cancelled = true;
    };
  }, []);

  if (ides.length === 0 || !selectedId || !workingDir) return null;

  const selected = ides.find((ide) => ide.id === selectedId);
  const selectedName = selected?.name ?? intl.formatMessage(i18n.fallbackName);

  const open = async (id: IdeId) => {
    try {
      await window.electron.openInIde(id, { dir: workingDir });
    } catch (error) {
      console.error('Failed to open project in IDE:', error);
      toastError({
        title: intl.formatMessage(i18n.openFailed),
        msg: error instanceof Error ? error.message : String(error),
      });
    }
  };

  const selectDefaultAndOpen = async (id: IdeId) => {
    setSelectedId(id);
    try {
      await window.electron.setSetting('GOOSE_DEFAULT_IDE', id);
    } catch (error) {
      console.error('Failed to save default IDE:', error);
      toastError({
        title: intl.formatMessage(i18n.saveFailed),
        msg: error instanceof Error ? error.message : String(error),
      });
    }
    await open(id);
  };

  return (
    <div className="flex items-center">
      <Button
        type="button"
        variant="ghost"
        size="sm"
        onClick={() => void open(selectedId)}
        aria-label={intl.formatMessage(i18n.openIn, { name: selectedName })}
        className="rounded-l-md rounded-r-none px-3 text-text-primary/70 hover:text-text-primary transition-colors"
      >
        {selected?.name}
      </Button>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            aria-label={intl.formatMessage(i18n.chooseDefault)}
            className="rounded-l-none rounded-r-md px-1 text-text-primary/70 hover:text-text-primary transition-colors"
          >
            <ChevronDown className="w-3 h-3" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          {ides.map((ide) => (
            <DropdownMenuItem key={ide.id} onSelect={() => void selectDefaultAndOpen(ide.id)}>
              {ide.name}
              {ide.id === selectedId && (
                <span className="ml-auto pl-4 text-xs text-text-secondary">
                  {intl.formatMessage(i18n.defaultLabel)}
                </span>
              )}
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
