export enum ROUTES {
  HOME = '/',
  MEDIA = '/media',
  CAMERA = '/camera',
  INFO = '/info',
  SETTINGS = '/settings',
  QUIT = 'quit'
}

export const indexToRoute: Record<number, string | 'quit'> = {
  0: ROUTES.HOME,
  1: ROUTES.MEDIA,
  2: ROUTES.CAMERA,
  3: ROUTES.INFO,
  4: ROUTES.SETTINGS,
  5: ROUTES.QUIT
}

export const routeToIndex: Record<string, number> = {
  [ROUTES.HOME]: 0,
  [ROUTES.MEDIA]: 1,
  [ROUTES.CAMERA]: 2,
  [ROUTES.INFO]: 3,
  [ROUTES.SETTINGS]: 4,
  [ROUTES.QUIT]: 5
}
