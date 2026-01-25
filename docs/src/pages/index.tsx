import type {ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Layout from '@theme/Layout';

import styles from './index.module.css';

export default function Home(): ReactNode {
  const {siteConfig} = useDocusaurusContext();
  return (
    <Layout
      title="Script - High Performance JavaScript Alternative"
      description="Script is a native programming language for building high-performance systems with JavaScript-inspired syntax, memory safety, and a zero-compromise toolchain.">
      <main className={clsx(styles.page)}>
        <section className={clsx(styles.heroShell)}>
          <div className="container">
            <div className={styles.heroInner}>
              <div>
                <div className={styles.eyebrow}>Script Language</div>
                <h1 className={styles.title}>
                  Write fast.
                  <br />
                  Run faster.
                </h1>
                <p className={styles.subtitle}>
                  Script is a native programming language for building
                  high‑performance systems with a familiar, JavaScript‑inspired
                  syntax and a zero‑compromise toolchain.
                </p>
                <div className={styles.heroActions}>
                  <Link
                    className={clsx('button button--primary button--lg', styles.primaryCta)}
                    to="/docs/intro">
                    Get started with the docs
                  </Link>
                  <Link
                    className={clsx('button button--secondary button--lg', styles.secondaryCta)}
                    to="/blog">
                    Read the latest updates
                  </Link>
                </div>
                <p className={styles.heroMeta}>
                  Native performance • Familiar tooling • Built for modern runtimes
                </p>
              </div>
              <aside className={styles.heroAside}>
                <div className={styles.heroAsideTitle}>What’s inside</div>
                <div className={styles.heroAsideGrid}>
                  <div className={styles.heroAsideCard}>
                    <div className={styles.heroAsideCardTitle}>Runtime</div>
                    <div className={styles.heroAsideCardBody}>
                      A compact, Bun‑inspired runtime tuned for Script’s execution model.
                    </div>
                  </div>
                  <div className={styles.heroAsideCard}>
                    <div className={styles.heroAsideCardTitle}>Compiler</div>
                    <div className={styles.heroAsideCardBody}>
                      Ahead‑of‑time compilation directly to native code for predictable speed.
                    </div>
                  </div>
                  <div className={styles.heroAsideCard}>
                    <div className={styles.heroAsideCardTitle}>Type System</div>
                    <div className={styles.heroAsideCardBody}>
                      A pragmatic, ergonomic type system that stays out of your way.
                    </div>
                  </div>
                  <div className={styles.heroAsideCard}>
                    <div className={styles.heroAsideCardTitle}>Tooling</div>
                    <div className={styles.heroAsideCardBody}>
                      A cohesive toolkit for building, testing, and shipping Script apps.
                    </div>
                  </div>
                </div>
              </aside>
            </div>
          </div>
        </section>

        <section className={styles.sectionShell}>
          <div className="container">
            <div className={styles.sectionHeader}>
              <div className={styles.sectionTitle}>Why Script</div>
              <p className={styles.sectionSubtitle}>
                A language and runtime designed for people who care about both performance and polish.
              </p>
            </div>

            <div className={styles.featureGrid}>
              <article className={styles.featureCard}>
                <div className={styles.featureLabel}>Core runtime</div>
                <h3 className={styles.featureTitle}>Drop‑in speed</h3>
                <p className={styles.featureBody}>
                  Ship native binaries with startup and throughput characteristics comparable to C and Rust.
                </p>
              </article>
              <article className={styles.featureCard}>
                <div className={styles.featureLabel}>Developer experience</div>
                <h3 className={styles.featureTitle}>Familiar syntax</h3>
                <p className={styles.featureBody}>
                  JavaScript‑inspired semantics, modern tooling, and tight feedback loops for everyday work.
                </p>
              </article>
              <article className={styles.featureCard}>
                <div className={styles.featureLabel}>Safety</div>
                <h3 className={styles.featureTitle}>Confident systems</h3>
                <p className={styles.featureBody}>
                  Leverage Script’s type system and tooling to catch entire classes of bugs before they ship.
                </p>
              </article>
              <article className={styles.featureCard}>
                <div className={styles.featureLabel}>Interoperability</div>
                <h3 className={styles.featureTitle}>Talks to your stack</h3>
                <p className={styles.featureBody}>
                  Integrate with existing ecosystems and infrastructure without rewriting everything at once.
                </p>
              </article>
            </div>
          </div>
        </section>
      </main>
    </Layout>
  );
}
