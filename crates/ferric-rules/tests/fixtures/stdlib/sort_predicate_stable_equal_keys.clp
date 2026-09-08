;; #343 pinned sort behavior: stable-equal-keys
(deffunction ascending (?a ?b) (> (div ?a 10) (div ?b 10)))
(deffunction descending (?a ?b) (< (div ?a 10) (div ?b 10)))
(deffacts startup (go))
(defrule exercise (go) =>
(printout t (sort ascending (create$ 21 11 22 12 23)) crlf)
(printout t (sort descending (create$ 21 11 22 12 23)) crlf)
)
