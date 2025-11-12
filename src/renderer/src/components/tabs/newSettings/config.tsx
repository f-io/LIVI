import { GeneralSettings, VideoSettings } from './componetns'

export const SETTINGS_CONFIG = [
  {
    title: 'General',
    component: GeneralSettings
  },
  {
    title: 'Video',
    component: VideoSettings
  },
  {
    title: 'Audio',
    component: () => <>789</>
  },
  {
    title: 'Advanced',
    component: () => <>qwe</>
  },
  {
    title: 'Key bindings',
    component: () => <>asd</>
  },
  {
    title: 'Other',
    component: () => <>zxc</>
  },
  {
    title: 'test',
    component: () => <>test</>
  },
  {
    title: 'test',
    component: () => <>test</>
  },
  {
    title: 'test',
    component: () => <>test</>
  },
  {
    title: 'test',
    component: () => <>test</>
  }
]
