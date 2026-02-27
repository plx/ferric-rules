(deftemplate MAIN::foo (slot x))
(defmodule MAIN (export deftemplate ?NONE))
(defmodule FOO (import MAIN ?NONE))
