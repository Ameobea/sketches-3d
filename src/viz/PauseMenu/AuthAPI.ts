export interface AuthAPI {
  logOutPlayer: () => Promise<void>;
  login: (params: { playerLogin: { username: string; password: string } }) => Promise<void>;
  createPlayer: (params: { playerLogin: { username: string; password: string } }) => Promise<void>;
  getPlayer: () => Promise<{ id?: number | string | null; username: string | null } | null>;
  refetchUser: () => Promise<{ id?: number | string | null; username: string | null } | null>;
  setUserLoggedOut?: () => void;
}
