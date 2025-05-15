const API_BASE_URL = '/api/metrics';

export const recordPlayCompletion = async (
  levelName: string,
  isWin: boolean,
  timeSeconds: number,
  userId: string | null
) =>
  fetch(`${API_BASE_URL}/event/play_completed`, {
    method: 'POST',
    body: JSON.stringify({ levelName, isWin, timeSeconds, userId }),
  });

export const recordRestart = async (levelName: string, userId: string | null, timeSeconds: number) =>
  fetch(`${API_BASE_URL}/event/level_restarted`, {
    method: 'POST',
    body: JSON.stringify({ levelName, userId, timeSeconds }),
  });

export const recordPortalTravel = async (destinationLevelName: string, userId: string | null) =>
  fetch(`${API_BASE_URL}/event/portal_travel`, {
    method: 'POST',
    body: JSON.stringify({ destinationLevelName, userId }),
  });
