;; An unknown function in a dormant seed fails load without dropping its field.
;; Level: interaction
;; Covers: assertion-expression, deffacts, compile-time-validation
(deffacts seed (value (unknown 1)))
