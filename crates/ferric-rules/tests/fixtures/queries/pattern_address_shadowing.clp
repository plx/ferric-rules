;; Issue #328: assigned-pattern addresses in RHS expressions.
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)))
(defrule probe ?f <- (item (value 10)) =>
  (do-for-fact ((?f item)) (= (fact-slot-value ?f value) 20)
    (printout t "query-shadow:" (fact-index ?f) crlf))
  (printout t "after-query:" (fact-index ?f) crlf)
  (loop-for-count (?f 1 1) do (printout t "loop-shadow:" ?f crlf))
  (printout t "after-loop:" (fact-index ?f) crlf)
  (progn$ (?f (create$ a b)) (printout t "progn-shadow:" ?f crlf))
  (printout t "after-progn:" (fact-index ?f) crlf)
  (bind ?f 42)
  (if (= ?f 42) then (printout t "rebind-visible" crlf)))
