import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { CodeBlock } from './CodeBlock';
import type { Components } from 'react-markdown';

interface MarkdownRendererProps {
  content: string;
}

export function MarkdownRenderer({ content }: MarkdownRendererProps) {
  const components: Components = {
    code({ className, children, ...props }) {
      const match = /language-(\w+)/.exec(className || '');
      const codeString = String(children).replace(/\n$/, '');

      // Check if it's an inline code (no language class and short)
      if (!match && !className) {
        return (
          <code className="bg-bg-tertiary px-1.5 py-0.5 rounded text-sm" {...props}>
            {children}
          </code>
        );
      }

      return <CodeBlock language={match?.[1]}>{codeString}</CodeBlock>;
    },
    pre({ children }) {
      return <>{children}</>;
    },
  };

  return (
    <div className="prose max-w-none">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
        {content}
      </ReactMarkdown>
    </div>
  );
}
