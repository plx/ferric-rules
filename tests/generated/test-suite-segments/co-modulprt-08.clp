(deftemplate MAIN::foo (slot x))
(defmodule MAIN (export ?NONE))
(defmodule FOO (import MAIN ?NONE))
