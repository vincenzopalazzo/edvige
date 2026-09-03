import { currentLocale } from '../i18n';

export function formatMessageTimestamp(timestamp?: number): string {
  const date = timestamp ? new Date(timestamp * 1000) : new Date();
  const now = new Date();

  // Format time using locale's default hour cycle
  const timeStr = date.toLocaleTimeString(currentLocale, {
    hour: 'numeric',
    minute: '2-digit',
  });

  // Check if the message is from today
  if (
    date.getDate() === now.getDate() &&
    date.getMonth() === now.getMonth() &&
    date.getFullYear() === now.getFullYear()
  ) {
    return timeStr;
  }

  // If not today, format as localized date + time
  const dateStr = date.toLocaleDateString(currentLocale, {
    month: '2-digit',
    day: '2-digit',
    year: 'numeric',
  });

  return `${dateStr} ${timeStr}`;
}

export function formatClockDisplay(date: Date): { time: string; meridiem: string; hour: number } {
  const hour = date.getHours();
  const time = date.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
  // extract meridiem correctly
  const parts = time.split(' ');
  const mer = parts.length > 1 ? parts[-1] : '';
  const t = parts[0] || time;
  return { time: t, meridiem: mer, hour };
}

