;; #343 pinned sort behavior: imported-comparator-name
(defmodule M (export deffunction exchange))
(deffunction exchange (?a ?b) (> ?a ?b))
(defmodule MAIN (import M deffunction exchange))
(deffacts startup (go))
(defrule exercise (go) =>
(printout t (sort exchange (create$ 3 1 2)) crlf)
)
