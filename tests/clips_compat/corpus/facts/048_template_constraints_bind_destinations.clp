(deftemplate sample (slot fixed (default (bind ?x 7))) (slot dynamic (default-dynamic (bind ?y 8))))
(deffacts seed (sample))
(defrule show (sample (fixed ?a) (dynamic ?b)) => (printout t ?a ":" ?b crlf))
