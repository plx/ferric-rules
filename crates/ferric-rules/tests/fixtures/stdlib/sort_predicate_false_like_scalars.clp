;; Only the FALSE symbol suppresses exchange; these are different values.
(deffunction string-false (?left ?right) "FALSE")
(deffunction float-zero (?left ?right) 0.0)
(deffacts startup (go))
(defrule exercise (go) =>
 (printout t (sort string-false (create$ 3 1 2)) ":"
  (sort float-zero (create$ 3 1 2)) crlf))
