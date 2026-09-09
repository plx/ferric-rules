(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)) (item (value 30)))
(defglobal ?*replacement* = FALSE)
(defrule probe =>
  (delayed-do-for-all-facts ((?f item)) TRUE
    (printout t ?f:value ":" (fact-existp ?f) ":" (eq ?f ?*replacement*) crlf)
    (if (= ?f:value 10) then
      (do-for-fact ((?later item)) (= ?later:value 20) (retract ?later))
      (assert (item (value 20)))
      (bind ?*replacement* (nth$ 1 (find-fact ((?new item)) (= ?new:value 20))))))
  (do-for-fact ((?live item)) (= ?live:value 20)
    (printout t "replacement:" ?live:value ":" (fact-existp ?live) ":" (eq ?live ?*replacement*) crlf)))
