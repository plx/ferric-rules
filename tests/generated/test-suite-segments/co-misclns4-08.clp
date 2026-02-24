(defmodule FOO (export ?ALL))
(deftemplate FOO::foo)
(defmodule BAR (export ?ALL) (import FOO ?ALL))
(deftemplate BAR::foo)
