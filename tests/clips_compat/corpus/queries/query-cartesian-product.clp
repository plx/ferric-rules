;; Multiple query bindings enumerate matching fact tuples.
;; Level: interaction
;; Covers: queries, query-cartesian-product
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defglobal ?*count* = 0)
(defrule probe =>
    (do-for-all-facts ((?a item) (?b item)) (< ?a:value ?b:value) (bind ?*count* (+ ?*count* 1)))
    (printout t ?*count* crlf))
