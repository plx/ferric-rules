;; The loop's ordinary ?f targets item20 while compact ?f:value remains item10.
(deftemplate item (slot value))
(deffacts seed (item (value 10)) (item (value 20)))
(defrule probe =>
  (do-for-fact ((?f item)) (= ?f:value 10)
    (bind ?other (nth$ 1 (find-fact ((?g item)) (= ?g:value 20))))
    (progn$ (?f (create$ ?other))
      (retract ?f)
      (printout t "loop:" ?f:value ":" (fact-existp ?f) crlf))
    (printout t "query:" ?f:value ":" (fact-existp ?f) crlf))
  (printout t "remaining:" (length$ (find-all-facts ((?g item)) TRUE)) crlf))
