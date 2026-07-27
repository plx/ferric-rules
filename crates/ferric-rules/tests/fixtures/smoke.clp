(deffacts startup (greeting hello))

(defrule respond
   (greeting ?g)
   =>
   (printout t "Got: " ?g crlf))
