;; Reset; retract item30; assert item5; optionally snapshot/restore; assert item1.
;; Run, reset, run. The golden combines both runs.
(deftemplate item (slot value))
(deffacts seed (item (value 30)) (item (value 10)) (item (value 20)))
(defrule probe =>
  (printout t "first:" (fact-slot-value (nth$ 1 (find-fact ((?f item)) TRUE)) value) crlf)
  (bind ?all (find-all-facts ((?f item)) TRUE))
  (printout t "all:" (length$ ?all) ":")
  (progn$ (?entry ?all) (printout t (fact-slot-value ?entry value) ":"))
  (printout t crlf))
