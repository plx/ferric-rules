;; The shared numeric evaluator handles callable, test CE, and positive slot predicates.
(deftemplate item (slot value))
(defglobal ?*tests* = 0 ?*predicates* = 0)
(deffacts input (item (value 1)) (item (value 2)) (item (value 3)))
(deffunction between (?value) (< 0 ?value 4))
(defrule test-context
 (item (value ?value))
 (test (< 0 ?value 3))
 => (bind ?*tests* (+ ?*tests* 1)))
(defrule predicate-context
 (item (value ?value&:(<= 1 ?value 2)))
 => (bind ?*predicates* (+ ?*predicates* 1)))
(defrule summary (declare (salience -10))
 => (printout t ?*tests* ":" ?*predicates* ":" (between 2) ":" (between 4) crlf))
