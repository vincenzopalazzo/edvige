/**
 * @vitest-environment jsdom
 */
import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import Hub from './Hub';
import { IntlTestWrapper } from '../i18n/test-utils';
import type { FixedExtensionEntry } from './ConfigContext';
import { createSession } from '../sessions';
import { UserInput } from '../types/message';

type ChatInputCapture = {
  draftRef?: { current: string };
  handleSubmit: (input: UserInput) => void;
};

const captured = vi.hoisted(() => ({ chatInput: null as ChatInputCapture | null }));
const mockSetView = vi.fn();

vi.mock('./ConfigContext', () => ({
  useConfig: () => ({
    extensionsList: [
      {
        name: 'developer',
        type: 'builtin',
        description: 'developer',
        enabled: true,
      },
      {
        name: 'memory',
        type: 'builtin',
        description: 'memory',
        enabled: false,
      },
    ] satisfies FixedExtensionEntry[],
  }),
}));

vi.mock('./ChatInput', () => ({
  default: (props: ChatInputCapture & { handleSubmit: ChatInputCapture['handleSubmit'] }) => {
    captured.chatInput = props;
    return (
      <button
        type="button"
        onClick={() => props.handleSubmit({ msg: 'hello from hub', images: [] })}
      >
        Submit
      </button>
    );
  },
}));

vi.mock('../sessions', () => ({
  createSession: vi.fn(),
}));

vi.mock('../utils/workingDir', () => ({
  getInitialWorkingDir: () => '/tmp/hub-dir',
  getEffectiveWorkingDir: () => Promise.resolve('/tmp/hub-dir'),
}));

describe('Hub', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    captured.chatInput = null;
    Object.defineProperty(window, 'appConfig', {
      configurable: true,
      writable: true,
      value: {
        get: (key: string) => (key === 'GOOSE_WORKING_DIR' ? '/tmp/hub-dir' : null),
      },
    });
  });

  it('hands the draft to the input', () => {
    const draftRef = { current: 'a half-written thought' };
    render(
      <IntlTestWrapper>
        <Hub setView={mockSetView} draftRef={draftRef} />
      </IntlTestWrapper>
    );

    expect(captured.chatInput?.draftRef).toBe(draftRef);
  });

  it('navigates to pair immediately without waiting for createSession', () => {
    const draftRef = { current: 'hello from hub' };
    render(
      <IntlTestWrapper>
        <Hub setView={mockSetView} draftRef={draftRef} />
      </IntlTestWrapper>
    );

    fireEvent.click(screen.getByRole('button', { name: 'Submit' }));

    expect(createSession).not.toHaveBeenCalled();
    expect(draftRef.current).toBe('');
    expect(mockSetView).toHaveBeenCalledWith('pair', {
      disableAnimation: true,
      initialMessage: { msg: 'hello from hub', images: [] },
      workingDir: '/tmp/hub-dir',
      allExtensions: [
        {
          name: 'developer',
          type: 'builtin',
          description: 'developer',
          enabled: true,
        },
        {
          name: 'memory',
          type: 'builtin',
          description: 'memory',
          enabled: false,
        },
      ],
    });
  });
});
