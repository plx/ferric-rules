(deftemplate item (slot v))
(deffacts seed (item (v 1)) (item (v 2)))
(defrule run ?f <- (item (v 1)) ?other <- (item (v 2)) =>
  (bind ?again (nth$ 1 (find-fact ((?g item)) (= ?g:v 1))))
  (printout t ?f ":" ?other ":" (eq ?f ?again) ":" (eq ?f ?other) ":" (neq ?f ?other) crlf)
  (printout t (eq ?f (fact-index ?f)) ":" (integerp ?f) ":" (numberp ?f) ":" (symbolp ?f) ":" (stringp ?f) ":" (multifieldp ?f) crlf))
