;; find-all-facts filters candidates with a slot predicate.
;; Level: boundary
;; Covers: queries, find-all-facts-filter
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defrule probe =>
    (printout t (length$ (find-all-facts ((?f item)) (>= ?f:value 20))) crlf))