import { useEffect, useState } from 'react';
import { defineMessages, useIntl } from '../../../i18n';
import { IDE_CATALOG } from '../../../ide/registry';
import { type IdeId } from '../../../types/ide';
import { toastError } from '../../../toasts';

const i18n = defineMessages({
  title: {
    id: 'ideSettingsSection.title',
    defaultMessage: 'Default IDE',
  },
  description: {
    id: 'ideSettingsSection.description',
    defaultMessage: 'Choose the editor Goose uses when opening your project',
  },
  notDetected: {
    id: 'ideSettingsSection.notDetected',
    defaultMessage: 'not detected',
  },
  saveFailed: {
    id: 'ideSettingsSection.saveFailed',
    defaultMessage: 'Failed to save default IDE',
  },
});

export const IdeSettingsSection = () => {
  const intl = useIntl();
  const [selectedId, setSelectedId] = useState<string>('');
  const [detectedIds, setDetectedIds] = useState<Set<IdeId>>(new Set());

  useEffect(() => {
    async function loadIdes() {
      try {
        const [detected, savedId] = await Promise.all([
          window.electron.listDetectedIdes(),
          window.electron.getSetting('GOOSE_DEFAULT_IDE'),
        ]);
        const detectedIds = new Set(detected.map((ide) => ide.id));
        setDetectedIds(detectedIds);
        setSelectedId(detectedIds.has(savedId as IdeId) ? savedId : (detected[0]?.id ?? ''));
      } catch (error) {
        console.error('Error loading IDE settings:', error);
      }
    }
    loadIdes();
  }, []);

  const handleSelect = async (id: IdeId) => {
    setSelectedId(id);
    try {
      await window.electron.setSetting('GOOSE_DEFAULT_IDE', id);
    } catch (error) {
      console.error('Error saving default IDE:', error);
      toastError({
        title: intl.formatMessage(i18n.saveFailed),
        msg: error instanceof Error ? error.message : String(error),
      });
    }
  };

  return (
    <div className="space-y-2 pb-8">
      <h3 className="text-text-primary">{intl.formatMessage(i18n.title)}</h3>
      <p className="text-xs text-text-secondary">{intl.formatMessage(i18n.description)}</p>
      <div className="space-y-1">
        {(Object.entries(IDE_CATALOG) as [IdeId, { name: string }][]).map(([id, entry]) => {
          const detected = detectedIds.has(id);
          const checked = selectedId === id;
          return (
            <div
              key={id}
              onClick={detected ? () => void handleSelect(id) : undefined}
              className={`group flex items-center justify-between text-sm py-2 px-2 rounded-lg transition-all ${
                checked
                  ? 'bg-background-secondary'
                  : detected
                    ? 'bg-background-primary hover:bg-background-secondary'
                    : ''
              } ${detected ? 'cursor-pointer' : 'cursor-not-allowed opacity-50'}`}
            >
              <span className="text-text-primary">{entry.name}</span>
              {!detected && (
                <span className="text-xs text-text-secondary">
                  {intl.formatMessage(i18n.notDetected)}
                </span>
              )}
              <input
                type="radio"
                name="defaultIde"
                value={id}
                checked={checked}
                disabled={!detected}
                onChange={() => void handleSelect(id)}
                className="peer sr-only"
              />
              <div className="relative flex items-center gap-2 h-4 w-4 rounded-full border border-border-primary transition-all duration-200 ease-in-out group-hover:border-border-primary peer-checked:border-[6px] peer-checked:border-black dark:peer-checked:border-white peer-checked:bg-white dark:peer-checked:bg-black"></div>
            </div>
          );
        })}
      </div>
    </div>
  );
};
