; RH-CORE-024: exists distinguishes per-candidate support without multiplying witnesses.
(deffacts seed (candidate a) (candidate b) (proof a one) (proof a two) (proof c stray))
(defrule eligible (candidate ?x) (exists (proof ?x ?witness)) => (printout t "eligible " ?x crlf) (assert (result ?x)))
