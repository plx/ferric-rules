;; do-for-all-facts visits every matching fact.
;; Level: basic
;; Covers: queries, do-for-all-facts-count
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defglobal ?*count* = 0)
(defrule probe =>
    (do-for-all-facts ((?f item)) TRUE (bind ?*count* (+ ?*count* 1)))
    (printout t ?*count* crlf))
