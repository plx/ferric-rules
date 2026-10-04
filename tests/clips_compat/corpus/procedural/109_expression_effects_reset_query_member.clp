(deftemplate p (slot v))
(deffacts seed (p (v 1)))
(defrule run =>
  (do-for-fact ((?x p)) TRUE
    (bind ?local 7)
    (printout t "before:" ?x ":" ?x:v crlf)
    (reset)
    (printout t "after:" ?x ":" ?x:v ":" (fact-index ?x) ":" ?local crlf))
  (printout t "done" crlf)
  (halt))
