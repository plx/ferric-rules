;; #343 pinned sort behavior: predicate-return-nil
(deffunction exchange (?a ?b) nil)
(deffacts startup (go))
(defrule exercise (go) =>
(printout t (sort exchange (create$ 3 1 2)) crlf)
)
