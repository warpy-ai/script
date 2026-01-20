import type {ReactNode} from 'react';
import clsx from 'clsx';
import styles from './FeatureCard.module.css';

type FeatureCardProps = {
  label?: string;
  title: string;
  children: ReactNode;
  className?: string;
};

export function FeatureCard({label, title, children, className}: FeatureCardProps): ReactNode {
  return (
    <article className={clsx(styles.card, className)}>
      {label ? <div className={styles.label}>{label}</div> : null}
      <h3 className={styles.title}>{title}</h3>
      <p className={styles.body}>{children}</p>
    </article>
  );
}

