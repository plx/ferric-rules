;; #343 pinned sort behavior: predicate-return-void
(deffunction exchange (?a ?b) (printout t "called;"))
(deffacts startup (go))
(defrule exercise (go) =>
(printout t (sort exchange (create$ 3 1 2)) crlf)
)
