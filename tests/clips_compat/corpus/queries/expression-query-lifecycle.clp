;; find-fact and find-all-facts follow assertion order after a retraction and
;; later assertions; each reset restores the deffacts order first.
(deftemplate item (slot value))
(deffacts seed (item (value 30)) (item (value 10)) (item (value 20)))
(defrule reorder (declare (salience 10))
  ?f <- (item (value 30))
  =>
  (retract ?f)
  (assert (item (value 5)))
  (assert (item (value 1))))
(defrule probe =>
  (printout t "first:" (fact-slot-value (nth$ 1 (find-fact ((?f item)) TRUE)) value) crlf)
  (bind ?all (find-all-facts ((?f item)) TRUE))
  (printout t "all:" (length$ ?all) ":")
  (progn$ (?entry ?all) (printout t (fact-slot-value ?entry value) ":"))
  (printout t crlf))
