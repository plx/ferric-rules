;; #343 pinned sort behavior: equivalent-callable-directions
(deffunction descending (?a ?b) (< ?a ?b))
(deffunction ascending (?a ?b) (> ?a ?b))
(deffacts startup (go))
(defrule exercise (go) =>
(printout t (sort < (create$ 3 1 2 1)) ":" (sort descending (create$ 3 1 2 1)) crlf)
(printout t (sort > (create$ 3 1 2 1)) ":" (sort ascending (create$ 3 1 2 1)) crlf)
)
