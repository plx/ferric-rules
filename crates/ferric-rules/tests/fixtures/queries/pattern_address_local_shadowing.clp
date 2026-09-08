;; Issue #328: scoped query/loop bindings override prior RHS locals and restore them.
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)))
(defrule probe ?f <- (item (value 10)) =>
  (bind ?f 42)
  (bind ?f-index 99)
  (do-for-fact ((?f item)) (= (fact-slot-value ?f value) 20)
    (printout t "query-local:" (fact-index ?f) crlf))
  (printout t "after-query-local:" ?f crlf)
  (loop-for-count (?f 1 1) do (printout t "loop-local:" ?f crlf))
  (printout t "after-loop-local:" ?f crlf)
  (progn$ (?f (create$ a b))
    (printout t "progn-local:" ?f ":" ?f-index crlf))
  (printout t "after-progn-local:" ?f ":" ?f-index crlf))
