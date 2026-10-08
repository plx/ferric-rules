(deffacts seed (a) (b) (c) (d))
(deffunction boom (?f) (printout t "boom called" crlf) ?f)
(defgeneric gen)
(defmethod gen ((?f FACT-ADDRESS)) (printout t "gen called" crlf) ?f)
(defrule run ?a <- (a) ?b <- (b) ?c <- (c) =>
  (printout t "before" crlf)
  (retract x (boom ?a) (MAIN::gen ?b) ?c (+ 0 4))
  (printout t "after" crlf))
