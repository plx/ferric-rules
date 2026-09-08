;; #343 pinned sort behavior: lexical-comparator-module
(defmodule M (export deffunction run-sort))
(deffunction exchange (?a ?b) (> ?a ?b))
(deffunction run-sort (?items) (sort exchange ?items))
(defmodule MAIN (import M deffunction run-sort))
(deffunction exchange (?a ?b) (< ?a ?b))
(deffacts startup (go))
(defrule exercise (go) =>
(printout t (sort exchange (create$ 3 1 2)) ":" (run-sort (create$ 3 1 2)) crlf)
)
