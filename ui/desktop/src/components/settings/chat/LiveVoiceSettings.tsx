import { useEffect, useState } from 'react';
import { useConfig } from '../../ConfigContext';
import { Switch } from '../../ui/switch';
import { defineMessages, useIntl } from '../../../i18n';

const LIVE_VOICE_ENABLED_CONFIG_KEY = 'GOOSE_LIVE_VOICE_ENABLED';
const LIVE_VOICE_PROVIDER_CONFIG_KEY = 'GOOSE_LIVE_VOICE_PROVIDER';

const i18n = defineMessages({
  title: {
    id: 'liveVoiceSettings.title',
    defaultMessage: 'Live voice',
  },
  description: {
    id: 'liveVoiceSettings.description',
    defaultMessage: 'Have a real-time voice conversation with Goose',
  },
  provider: {
    id: 'liveVoiceSettings.provider',
    defaultMessage: 'Voice provider',
  },
  grok: {
    id: 'liveVoiceSettings.grok',
    defaultMessage: 'Grok',
  },
  openai: {
    id: 'liveVoiceSettings.openai',
    defaultMessage: 'OpenAI',
  },
  grokHint: {
    id: 'liveVoiceSettings.grokHint',
    defaultMessage: 'Grok uses the xAI API key and requires Autonomous mode.',
  },
});

export const LiveVoiceSettings = () => {
  const intl = useIntl();
  const { read, upsert } = useConfig();
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [provider, setProvider] = useState<'openai' | 'xai'>('openai');

  useEffect(() => {
    void read(LIVE_VOICE_ENABLED_CONFIG_KEY, false).then((value) => {
      setEnabled(value === true || value === 1 || value === '1' || value === 'true');
    });
    void read(LIVE_VOICE_PROVIDER_CONFIG_KEY, false).then((value) => {
      setProvider(value === 'xai' || value === 'grok' ? 'xai' : 'openai');
    });
  }, [read]);

  const handleToggle = async (checked: boolean) => {
    await upsert(LIVE_VOICE_ENABLED_CONFIG_KEY, checked, false);
    setEnabled(checked);
  };

  return (
    <div>
      <div className="flex items-center justify-between py-2 px-2 hover:bg-background-secondary rounded-lg transition-all">
        <div>
          <h3 className="text-text-primary">{intl.formatMessage(i18n.title)}</h3>
          <p className="text-xs text-text-secondary max-w-md mt-[2px]">
            {intl.formatMessage(i18n.description)}
          </p>
        </div>
        <Switch
          checked={enabled ?? false}
          disabled={enabled === null}
          onCheckedChange={handleToggle}
          variant="mono"
          aria-label={intl.formatMessage(i18n.title)}
        />
      </div>
      <label className="flex items-center justify-between gap-3 py-2 px-2">
        <span className="text-sm text-text-secondary">
          {intl.formatMessage(i18n.provider)}
          {provider === 'xai' ? ` — ${intl.formatMessage(i18n.grokHint)}` : ''}
        </span>
        <select
          className="bg-background-primary border border-border-primary rounded-md px-2 py-1 text-sm"
          value={provider}
          aria-label={intl.formatMessage(i18n.provider)}
          onChange={(event) => {
            const next = event.target.value === 'xai' ? 'xai' : 'openai';
            setProvider(next);
            void upsert(LIVE_VOICE_PROVIDER_CONFIG_KEY, next, false);
          }}
        >
          <option value="openai">{intl.formatMessage(i18n.openai)}</option>
          <option value="xai">{intl.formatMessage(i18n.grok)}</option>
        </select>
      </label>
    </div>
  );
};
