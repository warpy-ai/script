import type {ReactNode} from 'react';
import Layout from '@theme/Layout';

type SiteShellProps = {
  title?: string;
  description?: string;
  children: ReactNode;
};

export function SiteShell({title, description, children}: SiteShellProps): ReactNode {
  return (
    <Layout title={title} description={description}>
      {children}
    </Layout>
  );
}

