(deffacts seed (u 3))
(defrule older (u ?) => (printout t OLDER crlf))
(defrule install (declare (salience 1000)) =>
  (build "(defrule newer (u ?) => (printout t NEWER crlf))"))
