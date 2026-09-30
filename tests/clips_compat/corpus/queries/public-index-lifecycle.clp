;; Issue #329: a retracted query address reports index -1, survivors keep
;; their indices, and a new fact takes the next index.
(deftemplate item (slot value))
(defglobal ?*saved* = FALSE)
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defrule capture (declare (salience 20)) =>
  (do-for-fact ((?f item)) (= (fact-slot-value ?f value) 10)
    (bind ?*saved* ?f)
    (printout t "saved:" (fact-index ?f) crlf)))
(defrule replace (declare (salience 10)) =>
  (retract ?*saved*)
  (assert (item (value 40))))
(defrule resumed =>
  (printout t "stale:" (fact-index ?*saved*) crlf)
  (do-for-fact ((?f item)) (= (fact-slot-value ?f value) 20)
    (printout t "20:" (fact-index ?f) crlf))
  (do-for-fact ((?f item)) (= (fact-slot-value ?f value) 30)
    (printout t "30:" (fact-index ?f) crlf))
  (do-for-fact ((?f item)) (= (fact-slot-value ?f value) 40)
    (printout t "40:" (fact-index ?f) crlf)))
