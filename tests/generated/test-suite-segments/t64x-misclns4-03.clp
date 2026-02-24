(defmodule d3 (export ?ALL))
(deftemplate t1)
(deftemplate t2)
(defmodule d4 (export deftemplate t3 t4) (import d3 deftemplate t1))
